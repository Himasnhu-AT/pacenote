//! The full-screen map (screen id [`SCREEN`]): the whole region, the village, every event
//! start with its name, your car, and the route to the picked event; a list of the events on
//! the right. Opened with `M` / d-pad up in free roam, or Map on the pause menu. Free roam is
//! paused while it's open (it is over the pause menu when opened from there).
//!
//! Controls (keyboard / gamepad): Q / E or LB / RB pick an event, Enter / A sets (or clears)
//! the route to it, arrows / WASD / left stick pan, + / − or RT / LT (and the mouse wheel)
//! zoom, F / X centres on the car, Esc / B or M closes. The mouse drags to pan and clicks a
//! marker to pick it.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use engine::actions::{ActionDef, ActionState, AddAction, Binding, Device};
use engine::pause::PauseCtl;
use engine::ui::screens::{RegisterScreen, ScreenRequest, Screens};
use engine::ui::theme::*;

use super::{MapArt, ROUTE, SCREEN, Waypoint, car_pose, distance_label, kind_color, paint, start_of};
use crate::drive::DrivenCar;
use crate::open::RoadKind;
use crate::open::events::Events;
use crate::prefs::Prefs;
use crate::world::StageWorld;

/// Width of the event list on the right (UI px).
const PANEL: f32 = 430.0;
/// Most zoom (× the whole-region fit).
const MAX_ZOOM: f32 = 8.0;

/// Where the full map looks (kept between openings).
#[derive(Resource, Clone, Debug)]
struct View {
    /// Map point (uv) at the frame's centre.
    centre: Vec2,
    /// × the scale that fits the whole region in the frame.
    zoom: f32,
    /// Easing towards this centre (after picking an event or centring on the car).
    goal: Option<Vec2>,
    /// The picked event (index into `Events`).
    sel: usize,
}

impl Default for View {
    fn default() -> Self {
        View { centre: Vec2::splat(0.5), zoom: 1.0, goal: None, sel: 0 }
    }
}

#[derive(Component)]
struct Frame;
#[derive(Component)]
struct FullImage;
#[derive(Component)]
struct FullRoute(u32);
#[derive(Component)]
struct FullCar;
/// An event's marker (index into `Events`) and its name label.
#[derive(Component)]
struct FullMarker(usize);
#[derive(Component)]
struct FullLabel(usize);
#[derive(Component)]
struct FlagRing;
/// A row of the event list, and its "route set" note.
#[derive(Component)]
struct Row(usize);
#[derive(Component)]
struct RowRoute(usize);

pub(super) fn build(app: &mut App) {
    app.init_resource::<View>()
        .register_screen(SCREEN, build_screen)
        .add_action(ActionDef::button("worldmap_zoom_in").label("Map zoom in").group("Map").context("menu").bind(Binding::key(KeyCode::Equal)).bind(Binding::key(KeyCode::NumpadAdd)).bind(Binding::pad(GamepadButton::RightTrigger2)))
        .add_action(ActionDef::button("worldmap_zoom_out").label("Map zoom out").group("Map").context("menu").bind(Binding::key(KeyCode::Minus)).bind(Binding::key(KeyCode::NumpadSubtract)).bind(Binding::pad(GamepadButton::LeftTrigger2)))
        .add_action(ActionDef::button("worldmap_centre").label("Map: centre on the car").group("Map").context("menu").bind(Binding::key(KeyCode::KeyF)).bind(Binding::pad(GamepadButton::West)))
        .add_action(ActionDef::button("worldmap_close").label("Close the map").group("Map").context("menu").bind(Binding::key(KeyCode::KeyM)).bind(Binding::pad(GamepadButton::Select)))
        .add_systems(Update, hold_pause)
        .add_systems(Update, (controls, layout).chain().run_if(on_map).run_if(resource_exists::<StageWorld>));
}

fn on_map(screens: Option<Res<Screens>>) -> bool {
    screens.is_some_and(|s| s.top() == Some(SCREEN))
}

/// Opened from free roam (not over the pause menu), the map pauses the game itself; closing
/// it resumes.
fn hold_pause(screens: Option<Res<Screens>>, state: Option<Res<State<engine::flow::AppState>>>, mut pause: PauseCtl, mut mine: Local<bool>) {
    let in_game = state.is_some_and(|s| *s.get() == engine::flow::AppState::InGame);
    let stack = screens.as_ref().map(|s| s.stack.as_slice()).unwrap_or(&[]);
    let open = in_game && stack.contains(&SCREEN) && !stack.contains(&"pause");
    if open && !*mine {
        *mine = true;
        pause.set(true);
    } else if !open && *mine {
        *mine = false;
        if !stack.contains(&"pause") {
            pause.set(false);
        }
    }
}

fn build_screen(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let gamepad = world.get_resource::<ActionState>().is_some_and(|a| a.last_device == Device::Gamepad);
    let stage = world.get_resource::<StageWorld>().cloned();
    let art = world.get_resource::<MapArt>().cloned();
    let events = world.get_resource::<Events>().cloned().unwrap_or_default();
    let car = world.query::<&DrivenCar>().iter(world).next().map(car_pose);
    let wp = world.resource::<Waypoint>().clone();
    let free = stage.as_ref().is_some_and(|s| s.free_roam());
    let map = art.as_ref().and_then(|a| a.map.clone());
    // Open on the car (or the region), with the picked or nearest event selected.
    {
        let mut v = world.resource_mut::<View>();
        if let Some((p, _)) = car {
            v.centre = paint::uv(p);
        }
        v.goal = None;
        let pick = wp.event.and_then(|id| events.0.iter().position(|e| e.id == id)).or_else(|| {
            let p = car?.0;
            (0..events.0.len()).min_by(|&a, &b| p.distance(start_of(&events.0[a])).total_cmp(&p.distance(start_of(&events.0[b]))))
        });
        v.sel = pick.unwrap_or(0).min(events.0.len().saturating_sub(1));
    }
    let village = stage.as_ref().and_then(|s| s.open.as_ref()).and_then(|o| {
        let pts: Vec<Vec2> = o.net.roads.iter().filter(|r| r.kind == RoadKind::Street).flat_map(|r| r.road.pts.iter().map(|p| Vec2::new(p.x, p.z))).collect();
        (!pts.is_empty()).then(|| pts.iter().copied().sum::<Vec2>() / pts.len() as f32)
    });
    let mph = world.get_resource::<Prefs>().is_some_and(|p| p.mph);
    // The roadside challenges: a star each, tinted by the stars earned.
    let star = world.get_resource::<crate::hud_challenge::icons::HudIcons>().map(|i| i.get(crate::hud_challenge::icons::Icon::Star));
    let challenges: Vec<(Vec2, Color)> = {
        let bests = world.get_resource::<crate::open::challenges::Bests>();
        world.get_resource::<crate::open::challenges::Challenges>().map_or(Vec::new(), |c| {
            c.0.iter().map(|p| (Vec2::new(p.first().p.x, p.first().p.z), crate::open::challenges::star_tint(bests.and_then(|b| b.get(p.def.id)).map_or(0, |b| b.stars)))).collect()
        })
    };

    world.commands().entity(root).with_children(|p| {
        p.spawn((Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }, BackgroundColor(Color::srgb(0.03, 0.035, 0.03)), Pickable::IGNORE));
        let (Some(map), Some(art), true) = (map, art.clone(), free) else {
            // During an event, on an old stage, or before the map is painted: no map.
            let (head, note) = if free {
                ("The map is being drawn", "Try again in a moment.")
            } else if stage.as_ref().is_some_and(|s| s.in_world()) {
                ("No map during an event", "The pace notes are your map. Leave the event from the pause menu to look around.")
            } else {
                ("No map on this stage", "The map shows the open world.")
            };
            p.spawn(menu_column(GAP_M)).with_children(|c| {
                c.spawn(eyebrow("Map", t.accent));
                c.spawn(title(head, SIZE_H1, t.text));
                c.spawn(text(note, body(SIZE_BODY), t.text_muted));
            });
            spawn_footer(p, &t, &[(if gamepad { "B" } else { "Esc" }, "Back")]);
            return;
        };
        // The map frame: everything left of the event list.
        p.spawn((
            Frame,
            Node { position_type: PositionType::Absolute, left: px(48), top: px(48), bottom: px(104), right: px(PANEL + 72.0), overflow: Overflow::clip(), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
            BorderColor::all(t.line_strong),
            BackgroundColor(Color::srgb(0.1, 0.12, 0.1)),
        ))
        .with_children(|f| {
            f.spawn((FullImage, Node { position_type: PositionType::Absolute, width: px(800), height: px(800), ..default() }, ImageNode::new(map), Pickable::IGNORE)).with_children(|img| {
                img.spawn((FullRoute(u32::MAX), Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }, Pickable::IGNORE));
                if let Some(v) = village {
                    let q = paint::uv(v);
                    img.spawn((Node { position_type: PositionType::Absolute, left: percent(q.x * 100.0), top: percent(q.y * 100.0), margin: UiRect::new(px(-60), px(0), px(36), px(0)), width: px(120), justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE)).with_children(|l| {
                        l.spawn((text("VILLAGE", label(SIZE_LABEL), t.text), TextShadow::default()));
                    });
                }
                if let Some(star) = &star {
                    for (at, c) in &challenges {
                        let q = paint::uv(*at);
                        img.spawn((Node { position_type: PositionType::Absolute, left: percent(q.x * 100.0), top: percent(q.y * 100.0), width: px(18), height: px(18), margin: UiRect::new(px(-9), px(0), px(-9), px(0)), ..default() }, ImageNode::new(star.clone()).with_color(*c), Pickable::IGNORE));
                    }
                }
                for (k, e) in events.0.iter().enumerate() {
                    let q = paint::uv(start_of(e));
                    // Events starting at the same place: their names stack under each other.
                    let stack = events.0[..k].iter().filter(|o| start_of(o).distance(start_of(e)) < 80.0).count();
                    img.spawn((FlagRing, FullMarker(k), Node { position_type: PositionType::Absolute, left: percent(q.x * 100.0), top: percent(q.y * 100.0), width: px(34), height: px(34), margin: UiRect::new(px(-17), px(0), px(-17), px(0)), display: Display::None, ..default() }, ImageNode::new(art.ring.clone()).with_color(t.text), Pickable::IGNORE));
                    img.spawn((FullMarker(k), Node { position_type: PositionType::Absolute, left: percent(q.x * 100.0), top: percent(q.y * 100.0), width: px(20), height: px(20), margin: UiRect::new(px(-10), px(0), px(-10), px(0)), ..default() }, ImageNode::new(art.dot.clone()).with_color(kind_color(e.kind)), Pickable::IGNORE));
                    img.spawn((
                        FullLabel(k),
                        Node { position_type: PositionType::Absolute, left: percent(q.x * 100.0), top: percent(q.y * 100.0), margin: UiRect::new(px(16), px(0), px(-13.0 + 24.0 * stack as f32), px(0)), padding: UiRect::axes(px(6), px(1)), column_gap: px(6), align_items: AlignItems::Center, border_radius: BorderRadius::all(px(3)), ..default() },
                        BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.7)),
                        Pickable::IGNORE,
                    ))
                    .with_children(|l| {
                        l.spawn((Node { width: px(7), height: px(7), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(kind_color(e.kind)), Pickable::IGNORE));
                        l.spawn(text(e.name.to_uppercase(), label(SIZE_TINY), t.text));
                    });
                }
                if let Some((pos, _)) = car {
                    let q = paint::uv(pos);
                    img.spawn((FullCar, Node { position_type: PositionType::Absolute, left: percent(q.x * 100.0), top: percent(q.y * 100.0), width: px(30), height: px(30), margin: UiRect::new(px(-15), px(0), px(-15), px(0)), ..default() }, UiTransform::default(), ImageNode::new(art.arrow.clone()), Pickable::IGNORE));
                }
            });
        });
        // The event list and the legend.
        p.spawn((Node { position_type: PositionType::Absolute, right: px(48), top: px(48), bottom: px(104), width: px(PANEL), flex_direction: FlexDirection::Column, row_gap: px(GAP_S), ..default() }, Pickable::IGNORE)).with_children(|c| {
            c.spawn(eyebrow("Map", t.accent));
            c.spawn(title("Events", SIZE_H2, t.text));
            c.spawn(spacer(GAP_XS));
            for (k, e) in events.0.iter().enumerate() {
                let d = car.map(|(p, _)| p.distance(start_of(e))).unwrap_or(0.0);
                c.spawn((
                    Row(k),
                    Node { flex_direction: FlexDirection::Column, padding: UiRect::new(px(14), px(12), px(8), px(8)), border: UiRect::left(px(3)), border_radius: BorderRadius::all(px(RADIUS)), row_gap: px(2), ..default() },
                    BackgroundColor(t.panel),
                    BorderColor::all(kind_color(e.kind)),
                    Pickable::IGNORE,
                ))
                .with_children(|r| {
                    r.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Baseline, ..default() }, Pickable::IGNORE)).with_children(|h| {
                        h.spawn(text(e.name.to_uppercase(), label(SIZE_LABEL), t.text));
                        h.spawn(text(distance_label(d, mph), label(SIZE_TINY + 1.0), t.text_muted));
                    });
                    r.spawn((Node { justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|h| {
                        h.spawn(text(format!("{} \u{b7} {}", e.kind.label().to_uppercase(), distance_label(e.route.length(), mph)), label(SIZE_TINY), kind_color(e.kind)));
                        h.spawn((RowRoute(k), text("", label(SIZE_TINY), ROUTE)));
                    });
                });
            }
            c.spawn(spacer(GAP_S));
            c.spawn((Node { flex_wrap: FlexWrap::Wrap, column_gap: px(GAP_M), row_gap: px(6), ..default() }, Pickable::IGNORE)).with_children(|l| {
                for (name, kind) in [("Tarmac", RoadKind::BRoad), ("Village", RoadKind::Street), ("Gravel", RoadKind::Gravel), ("Forest track", RoadKind::Forest)] {
                    let c = paint::road_color(kind);
                    swatch(l, &t, name, Color::srgb(c[0], c[1], c[2]));
                }
                swatch(l, &t, "Route", ROUTE);
                if let Some(star) = &star {
                    l.spawn((Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|s| {
                        for k in 0..4u8 {
                            s.spawn((Node { width: px(14), height: px(14), ..default() }, ImageNode::new(star.clone()).with_color(crate::open::challenges::star_tint(k)), Pickable::IGNORE));
                        }
                        s.spawn(text(format!("Challenges ({}) by stars", challenges.len()), label(SIZE_TINY), t.text_muted));
                    });
                }
            });
        });
        let hints: &[(&str, &str)] = if gamepad {
            &[("LB/RB", "Pick event"), ("A", "Set route"), ("LS", "Pan"), ("LT/RT", "Zoom"), ("X", "Your car"), ("B", "Close")]
        } else {
            &[("Q/E", "Pick event"), ("Enter", "Set route"), ("Arrows", "Pan"), ("+/\u{2212}", "Zoom"), ("F", "Your car"), ("M", "Close")]
        };
        spawn_footer(p, &t, hints);
    });
}

fn swatch(p: &mut ChildSpawnerCommands, t: &Theme, name: &str, c: Color) {
    p.spawn((Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|s| {
        s.spawn((Node { width: px(16), height: px(5), border_radius: BorderRadius::all(px(2)), ..default() }, BackgroundColor(c), Pickable::IGNORE));
        s.spawn(text(name, label(SIZE_TINY), t.text_muted));
    });
}

/// The frame's size and top-left (UI px, and physical px for the cursor) plus the scale
/// between them.
fn frame_rect(q: &Query<(&ComputedNode, &UiGlobalTransform), With<Frame>>) -> Option<(Vec2, Vec2, f32)> {
    let (n, g) = q.single().ok()?;
    let inv = n.inverse_scale_factor();
    let size_phys = n.size();
    let top_left_phys = g.translation - size_phys * 0.5;
    Some((size_phys * inv, top_left_phys, inv))
}

/// Side of the whole map image (UI px) at this zoom in a frame this size.
fn image_side(frame: Vec2, zoom: f32) -> f32 {
    frame.min_element().max(100.0) * zoom
}

#[allow(clippy::too_many_arguments)]
fn controls(
    time: Res<Time<Real>>,
    actions: Res<ActionState>,
    scroll: Res<AccumulatedMouseScroll>,
    motion: Res<AccumulatedMouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    events: Option<Res<Events>>,
    cars: Query<&DrivenCar>,
    frame: Query<(&ComputedNode, &UiGlobalTransform), With<Frame>>,
    mut view: ResMut<View>,
    mut wp: ResMut<Waypoint>,
    mut requests: MessageWriter<ScreenRequest>,
    mut drag: Local<f32>,
) {
    if actions.just_pressed("worldmap_close") {
        requests.write(ScreenRequest::Pop);
        return;
    }
    let Some((size, top_left, inv)) = frame_rect(&frame) else { return };
    let dt = time.delta_secs().min(0.1);
    let side = image_side(size, view.zoom);
    let n = events.as_ref().map_or(0, |e| e.0.len());
    // Pick an event: the map eases over to it.
    let step = i32::from(actions.just_pressed("ui.next_tab")) - i32::from(actions.just_pressed("ui.prev_tab"));
    if step != 0 && n > 0 {
        view.sel = (view.sel as i32 + step).rem_euclid(n as i32) as usize;
        view.goal = events.as_ref().map(|e| paint::uv(start_of(&e.0[view.sel])));
    }
    if actions.just_pressed("ui.accept")
        && let Some(e) = events.as_ref().and_then(|e| e.0.get(view.sel))
    {
        let id = (wp.event != Some(e.id)).then_some(e.id);
        wp.set(id);
    }
    if actions.just_pressed("worldmap_centre")
        && let Ok(c) = cars.single()
    {
        view.goal = Some(paint::uv(car_pose(c).0));
    }
    // Zoom: keys / triggers step, the wheel too.
    let mut z = 0.0;
    if actions.just_pressed("worldmap_zoom_in") {
        z += 1.0;
    }
    if actions.just_pressed("worldmap_zoom_out") {
        z -= 1.0;
    }
    if scroll.delta.y != 0.0 {
        z += match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
    }
    if z != 0.0 {
        view.zoom = (view.zoom * 1.4f32.powf(z)).clamp(1.0, MAX_ZOOM);
    }
    // Pan: stick / arrows at a screen speed, mouse drag one to one.
    let nav = actions.axis2d("ui.navigate");
    if nav.length() > 0.2 {
        let speed = 700.0 / side; // uv per second: ~700 UI px/s on screen
        view.centre += Vec2::new(nav.x, -nav.y) * speed * dt;
        view.goal = None;
    }
    let window = windows.iter().next();
    if buttons.just_pressed(MouseButton::Left) {
        *drag = 0.0;
    }
    if buttons.pressed(MouseButton::Left) && motion.delta != Vec2::ZERO {
        let scale = window.map_or(1.0, |w| w.scale_factor()) * inv;
        let d = motion.delta * scale;
        *drag += d.length();
        view.centre -= d / side;
        view.goal = None;
    }
    // A click (not a drag) on a marker picks it.
    if buttons.just_released(MouseButton::Left)
        && *drag < 6.0
        && let (Some(w), Some(ev)) = (window, events.as_ref())
        && let Some(c) = w.physical_cursor_position()
    {
        let local = (c - top_left) * inv;
        let img_tl = size * 0.5 - view.centre * side;
        let near = ev.0.iter().enumerate().map(|(k, e)| (k, (paint::uv(start_of(e)) * side + img_tl).distance(local))).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, d)) = near
            && d < 22.0
        {
            view.sel = k;
        }
    }
    if let Some(g) = view.goal {
        let k = 1.0 - (-dt * 6.0).exp();
        view.centre = view.centre.lerp(g, k);
        if view.centre.distance(g) * side < 0.5 {
            view.goal = None;
        }
    }
    // Keep some of the map in the frame.
    let half = size * 0.5 / side;
    let fit = |c: f32, h: f32| if h >= 0.5 { 0.5 } else { c.clamp(h, 1.0 - h) };
    view.centre = Vec2::new(fit(view.centre.x, half.x), fit(view.centre.y, half.y));
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn layout(
    mut commands: Commands,
    theme: Res<Theme>,
    view: Res<View>,
    wp: Res<Waypoint>,
    events: Option<Res<Events>>,
    cars: Query<&DrivenCar>,
    frame: Query<(&ComputedNode, &UiGlobalTransform), With<Frame>>,
    mut image: Query<&mut Node, (With<FullImage>, Without<FullCar>, Without<FullMarker>, Without<FullLabel>)>,
    mut car_q: Query<(&mut Node, &mut UiTransform), (With<FullCar>, Without<FullImage>, Without<FullMarker>)>,
    mut rings: Query<(&FullMarker, &mut Node), (With<FlagRing>, Without<FullImage>, Without<FullCar>, Without<FullLabel>)>,
    mut labels: Query<(&FullLabel, &mut BackgroundColor), Without<Row>>,
    mut rows: Query<(&Row, &mut BackgroundColor), Without<FullLabel>>,
    mut route_texts: Query<(&RowRoute, &mut Text)>,
    mut route: Query<(Entity, &mut FullRoute)>,
) {
    let Some((size, _, _)) = frame_rect(&frame) else { return };
    let side = image_side(size, view.zoom);
    for mut n in &mut image {
        n.width = px(side);
        n.height = px(side);
        n.left = px(size.x * 0.5 - view.centre.x * side);
        n.top = px(size.y * 0.5 - view.centre.y * side);
    }
    if let (Ok(c), Ok((mut n, mut t))) = (cars.single(), car_q.single_mut()) {
        let (p, h) = car_pose(c);
        let q = paint::uv(p);
        n.left = percent(q.x * 100.0);
        n.top = percent(q.y * 100.0);
        t.rotation = Rot2::radians(h.y.atan2(h.x) + std::f32::consts::FRAC_PI_2);
    }
    let flagged = wp.event.and_then(|id| events.as_ref()?.0.iter().position(|e| e.id == id));
    for (m, mut n) in &mut rings {
        let on = Some(m.0) == flagged || m.0 == view.sel;
        let d = if on { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
    for (l, mut bg) in &mut labels {
        let c = if l.0 == view.sel { with_alpha(theme.accent, 0.35) } else { Color::srgba(0.02, 0.03, 0.025, 0.7) };
        if bg.0 != c {
            bg.0 = c;
        }
    }
    for (r, mut bg) in &mut rows {
        let c = if r.0 == view.sel { theme.raised } else { theme.panel };
        if bg.0 != c {
            bg.0 = c;
        }
    }
    for (r, mut t) in &mut route_texts {
        let s = if Some(r.0) == flagged { "ROUTE SET" } else { "" };
        if t.0 != s {
            t.0 = s.into();
        }
    }
    if let Ok((e, mut r)) = route.single_mut()
        && r.0 != wp.version
    {
        r.0 = wp.version;
        commands.entity(e).despawn_children();
        if wp.path.len() > 1 {
            let mut pts = wp.path.clone();
            if let Ok(c) = cars.single() {
                pts[0] = car_pose(c).0;
            }
            commands.entity(e).with_children(|p| super::spawn_line(p, &pts, 5.0, ROUTE));
        }
    }
}
