//! The showroom: the 3D diorama behind the menus. The active car (or the Dealership's
//! focused car) on a gravel plinth in a floodlit service tent, with a slow camera orbit on the
//! hub and fixed framings on the car screens.
//!
//! **Owner: stream E (UI).** Design: `docs/festival/UX_PASS.md` §1 ("Background: live 3D
//! showroom") and §4 (framings), `DECISIONS.md` U6, `ART_BIBLE.md` §6.
//!
//! - Only under `AppState::MainMenu` (`DespawnOnExit`); nothing in headless runs.
//! - The menu first draws over a veil (the theme's background and the contour backdrop); the
//!   veil fades out over 0.5 s once the car's model is in and two frames have drawn, so the
//!   first-draw pipeline stall happens behind it.
//! - Framing follows the top screen: hub = 40 s orbit (9°/s) 6 m out, 1.1 m up, the car right
//!   of centre; Garage = 3/4 front at 5 m; Upgrades = low side at 4 m, turned toward the part
//!   ([`Shot::Part`]); Dealership = 3/4 front, car swapped as the focus moves. The camera
//!   eases between framings over ~0.6 s. Reduce motion: no orbit (3/4 front), cuts.
//! - Bodies (U6): the Saker R5 is `rallycar.glb`; other cars use the traffic body shapes in
//!   their paint.

use bevy::prelude::*;
use engine::flow::AppState;
use engine::ui::prelude::*;

use crate::art::traffic_car::{self, TrafficArt};
use crate::career::garage::{self, Body, Garage, Paint};

/// The 7 paints (the traffic cars' colours, `art::traffic_car`), sRGB, with names. On the
/// Saker R5 (the rally hatch) paint 0 is its works livery.
pub const PAINTS: [(&str, [f32; 3]); 7] = [
    ("White", [0.86, 0.86, 0.84]),
    ("Silver", [0.56, 0.58, 0.61]),
    ("Black", [0.035, 0.037, 0.045]),
    ("Red", [0.56, 0.06, 0.05]),
    ("Blue", [0.07, 0.17, 0.46]),
    ("Green", [0.07, 0.24, 0.14]),
    ("Yellow", [0.82, 0.62, 0.12]),
];

/// The car on the plinth: a roster id and its paint. Screens set it; the showroom swaps the
/// model when it changes.
#[derive(Resource, Clone, Debug, PartialEq, Default)]
pub struct ShowroomCar {
    pub model: String,
    pub paint: Paint,
}

/// How the camera frames the car, as the top screen wants it.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Default)]
pub enum Shot {
    /// The hub: slow orbit, car right of centre.
    #[default]
    Orbit,
    /// 3/4 front, centred (Garage, Dealership).
    Front,
    /// Low side, turned toward a part (`angle`: 0 = behind, 180 = in front).
    Part(f32),
}

pub struct ShowroomPlugin;

impl Plugin for ShowroomPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowroomCar>()
            .init_resource::<Shot>()
            .add_systems(OnEnter(AppState::MainMenu), spawn)
            .add_systems(Update, (follow_screens, swap_car, paint_car, frame, fade).chain().run_if(in_state(AppState::MainMenu)));
    }
}

#[derive(Component)]
struct ShowroomCam {
    /// Orbit angle (degrees; 0 = behind the car).
    angle: f32,
    /// Eased camera position and look target.
    pos: Vec3,
    look: Vec3,
    started: bool,
}

/// The car's parent (the model under it is swapped).
#[derive(Component)]
struct Plinth {
    shown: Option<ShowroomCar>,
    /// Real time the current model was spawned.
    since: f32,
}

/// The rally model waiting for its paint (after the rig made its own paint materials).
#[derive(Component)]
struct WantsPaint(Paint);

/// The veil over the 3D view until it's ready.
#[derive(Component)]
struct ShowroomVeil {
    /// Frames drawn since the car was ready.
    frames: u32,
    /// Real time the fade began.
    fading: Option<f32>,
    born: f32,
}

/// The scene only exists in windowed runs.
fn spawn(world: &mut World) {
    if !crate::art::visual(world) {
        return;
    }
    let t = world.resource::<Theme>().clone();
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let backdrop = world.resource::<super::stages::StageCatalog>().backdrop.clone();
    // The veil: the 2D menu look until the 3D view is ready.
    world
        .spawn((ShowroomVeil { frames: 0, fading: None, born: now }, fullscreen(), BackgroundColor(t.bg), GlobalZIndex(5), Pickable::IGNORE, DespawnOnExit(AppState::MainMenu), Name::new("showroom veil")))
        .with_children(|v| {
            if let Some(img) = backdrop {
                v.spawn((
                    ImageNode { image: img, color: Color::srgba(1.0, 1.0, 1.0, 0.6), ..default() },
                    Node { position_type: PositionType::Absolute, right: px(-220), top: percent(-30), height: percent(160), aspect_ratio: Some(1.0), ..default() },
                    Pickable::IGNORE,
                ));
            }
        });

    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    let ground = meshes.add(Circle::new(40.0));
    let plinth = meshes.add(Cylinder::new(3.4, 0.14));
    let lip = meshes.add(Cylinder::new(3.55, 0.06));
    let rings: Vec<Handle<Mesh>> = [4.3, 5.6, 7.2, 9.1].iter().map(|r| meshes.add(Annulus::new(*r, r + 0.025))).collect();
    let wall = meshes.add(Cylinder::new(15.0, 9.0));
    let pole = meshes.add(Cylinder::new(0.06, 9.0));
    // The game's own gravel, tiled (repeat sampler; the floor darker than the plinth).
    let server = world.resource::<AssetServer>().clone();
    let load = |p: &'static str, srgb: bool| -> Handle<Image> {
        server
            .load_builder()
            .with_settings(move |s: &mut bevy::image::ImageLoaderSettings| {
                s.is_srgb = srgb;
                s.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
                    address_mode_u: bevy::image::ImageAddressMode::Repeat,
                    address_mode_v: bevy::image::ImageAddressMode::Repeat,
                    anisotropy_clamp: 8,
                    ..bevy::image::ImageSamplerDescriptor::linear()
                });
            })
            .load(p)
    };
    let gravel_tex = load("textures/gravel/color.ktx2", true);
    let gravel_nrm = load("textures/gravel/normal.ktx2", false);
    let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
    let tiled = |base: Color, tiles: f32| StandardMaterial {
        base_color: base,
        base_color_texture: Some(gravel_tex.clone()),
        normal_map_texture: Some(gravel_nrm.clone()),
        perceptual_roughness: 0.95,
        uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(tiles)),
        ..default()
    };
    let gravel = mats.add(tiled(Color::srgb(0.32, 0.31, 0.29), 26.0));
    let stone = mats.add(tiled(Color::srgb(0.72, 0.68, 0.62), 3.2));
    let edge = mats.add(StandardMaterial { base_color: Color::srgb(0.09, 0.085, 0.08), perceptual_roughness: 0.8, ..default() });
    let a = t.accent.to_srgba();
    let contour = mats.add(StandardMaterial { base_color: Color::srgba(a.red, a.green, a.blue, 0.28), unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    let canvas = mats.add(StandardMaterial { base_color: Color::srgb(0.085, 0.11, 0.09), perceptual_roughness: 1.0, cull_mode: None, double_sided: true, ..default() });
    let steel = mats.add(StandardMaterial { base_color: Color::srgb(0.2, 0.21, 0.2), perceptual_roughness: 0.5, metallic: 0.6, ..default() });

    let root = world.spawn((Name::new("showroom"), Transform::default(), Visibility::default(), DespawnOnExit(AppState::MainMenu))).id();
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    world.entity_mut(root).with_children(|r| {
        r.spawn((Mesh3d(ground), MeshMaterial3d(gravel), Transform::from_rotation(flat)));
        r.spawn((Mesh3d(lip), MeshMaterial3d(edge), Transform::from_xyz(0.0, 0.03, 0.0)));
        r.spawn((Mesh3d(plinth), MeshMaterial3d(stone), Transform::from_xyz(0.0, 0.07, 0.0)));
        // Contour lines on the floor: the map-paper motif.
        for m in rings {
            r.spawn((Mesh3d(m), MeshMaterial3d(contour.clone()), Transform::from_xyz(0.0, 0.004, 0.0).with_rotation(flat), bevy::light::NotShadowCaster));
        }
        // The tent: a canvas wall, poles and lamp strips up high.
        r.spawn((Mesh3d(wall), MeshMaterial3d(canvas), Transform::from_xyz(0.0, 4.5, 0.0), bevy::light::NotShadowCaster));
        for k in 0..12 {
            let a = k as f32 / 12.0 * std::f32::consts::TAU;
            let p = Vec3::new(a.sin() * 14.7, 4.5, a.cos() * 14.7);
            r.spawn((Mesh3d(pole.clone()), MeshMaterial3d(steel.clone()), Transform::from_translation(p), bevy::light::NotShadowCaster));
        }
        // Key light: a floodlight up front-left, with shadows; a rim from behind; a soft fill.
        r.spawn((
            SpotLight { intensity: 2_600_000.0, range: 30.0, radius: 0.4, outer_angle: 0.62, inner_angle: 0.35, shadow_maps_enabled: true, color: Color::srgb(1.0, 0.95, 0.86), ..default() },
            Transform::from_xyz(-4.0, 7.5, -5.0).looking_at(Vec3::new(0.0, 0.4, 0.0), Vec3::Y),
        ));
        r.spawn((
            SpotLight { intensity: 1_400_000.0, range: 30.0, radius: 0.3, outer_angle: 0.55, inner_angle: 0.3, shadow_maps_enabled: false, color: Color::srgb(0.75, 0.85, 1.0), ..default() },
            Transform::from_xyz(3.5, 5.0, 6.5).looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y),
        ));
        r.spawn((PointLight { intensity: 220_000.0, range: 25.0, radius: 1.0, shadow_maps_enabled: false, color: Color::srgb(1.0, 0.9, 0.75), ..default() }, Transform::from_xyz(5.0, 3.0, -2.0)));
        r.spawn((Plinth { shown: None, since: now }, Transform::from_xyz(0.0, 0.14, 0.0), Visibility::default(), Name::new("showroom car")));
    });
    world.spawn((
        Name::new("showroom camera"),
        ShowroomCam { angle: 140.0, pos: Vec3::ZERO, look: Vec3::ZERO, started: false },
        Camera3d::default(),
        Camera { clear_color: ClearColorConfig::Custom(t.bg), ..default() },
        Msaa::Off,
        bevy::camera::Exposure { ev100: 7.6 },
        AmbientLight { color: Color::srgb(0.75, 0.82, 0.78), brightness: 260.0, ..default() },
        DistanceFog { color: t.bg, falloff: FogFalloff::Linear { start: 14.0, end: 26.0 }, ..default() },
        Transform::from_xyz(0.0, 1.1, 6.0).looking_at(Vec3::Y * 0.6, Vec3::Y),
        DespawnOnExit(AppState::MainMenu),
    ));
}

/// The framing and the car each screen wants (the screens' own pick wins: Dealership sets
/// the focused car itself).
fn follow_screens(screens: Option<Res<Screens>>, garage: Option<Res<Garage>>, mut shot: ResMut<Shot>, mut car: ResMut<ShowroomCar>) {
    let top = screens.as_ref().and_then(|s| s.top());
    let under = screens.as_ref().and_then(|s| s.stack.len().checked_sub(2).map(|i| s.stack[i]));
    let top = if top == Some(super::confirm::SCREEN) { under } else { top };
    let want = match top {
        Some("garage" | "dealer") => Shot::Front,
        Some("upgrades") => match *shot {
            Shot::Part(a) => Shot::Part(a),
            _ => Shot::Part(110.0),
        },
        _ => Shot::Orbit,
    };
    if *shot != want {
        *shot = want;
    }
    // Outside the Dealership and the Garage (which pick their own), the active car.
    if !matches!(top, Some("dealer" | "garage"))
        && let Some(g) = garage
    {
        let a = g.active_car();
        let next = ShowroomCar { model: a.model.clone(), paint: a.paint };
        if *car != next {
            *car = next;
        }
    }
}

fn swap_car(world: &mut World) {
    let want = world.resource::<ShowroomCar>().clone();
    let mut q = world.query::<(Entity, &Plinth)>();
    let Some((plinth, shown)) = q.iter(world).next().map(|(e, p)| (e, p.shown.clone())) else { return };
    if shown.as_ref() == Some(&want) {
        return;
    }
    // Same model, new paint: just repaint.
    let def = garage::def(&want.model).unwrap_or(&garage::CARS[0]);
    if let Some(s) = &shown
        && s.model == want.model
        && def.body == Body::Rally
    {
        let kids: Vec<Entity> = world.get::<Children>(plinth).map(|c| c.iter().collect()).unwrap_or_default();
        for k in kids {
            world.entity_mut(k).insert(WantsPaint(want.paint));
        }
        world.get_mut::<Plinth>(plinth).unwrap().shown = Some(want);
        return;
    }
    world.entity_mut(plinth).despawn_related::<Children>();
    let now = world.resource::<Time<Real>>().elapsed_secs();
    {
        let mut p = world.get_mut::<Plinth>(plinth).unwrap();
        p.shown = Some(want.clone());
        p.since = now;
    }
    match def.body {
        Body::Rally => {
            let scene = world.resource::<AssetServer>().load(GltfAssetLabel::Scene(0).from_asset(crate::art::car::MODEL));
            // The model's origin is at hub height.
            let tf = Transform::from_xyz(0.0, crate::art::car::MODEL_WHEEL_RADIUS, 0.0);
            let model = world.spawn((Name::new("showroom model"), crate::art::car::CarModel, WorldAssetRoot(scene), tf, Visibility::default(), WantsPaint(want.paint))).id();
            world.entity_mut(plinth).add_child(model);
        }
        body => {
            if !world.contains_resource::<TrafficArt>() {
                let art = world.resource_scope(|w, mut meshes: Mut<Assets<Mesh>>| {
                    let mut mats = w.resource_mut::<Assets<StandardMaterial>>();
                    TrafficArt::new(&mut meshes, &mut mats)
                });
                world.insert_resource(art);
            }
            let shape = match body {
                Body::Hatch => 0,
                Body::Saloon => 1,
                Body::Estate => 2,
                _ => 3,
            };
            // A traffic id with this body shape and finish (`traffic_car::spawn_parts` derives
            // both from the id).
            let id = (0..4096u32).find(|id| traffic_car::shape_of(*id) == shape && ((id >> 3) & 1 == 1) == want.paint.metallic).unwrap_or(0);
            let model = world.spawn((Name::new("showroom model"), Transform::default(), Visibility::default())).id();
            world.entity_mut(plinth).add_child(model);
            world.resource_scope(|w, art: Mut<TrafficArt>| {
                w.commands().entity(model).with_children(|p| traffic_car::spawn_parts(p, &art, id, want.paint.colour));
            });
            world.flush();
        }
    }
}

/// The rally model's paint as built (to go back to the livery).
#[derive(Component)]
struct PaintOrig(Vec<(Handle<StandardMaterial>, Color, f32)>);

/// Repaints the rally model once its rig made its own paint materials (the meshes whose glTF
/// material is "Paint").
fn paint_car(world: &mut World) {
    let mut q = world.query_filtered::<(Entity, &WantsPaint), With<crate::art::car::CarRig>>();
    let todo: Vec<(Entity, Paint)> = q.iter(world).map(|(e, w)| (e, w.0)).collect();
    for (model, paint) in todo {
        if world.get::<PaintOrig>(model).is_none() {
            let mut found = Vec::new();
            let mut kids = vec![model];
            while let Some(e) = kids.pop() {
                if let Some(c) = world.get::<Children>(e) {
                    kids.extend(c.iter());
                }
                let is_paint = world.get::<bevy::gltf::GltfMaterialName>(e).is_some_and(|n| n.0 == "Paint");
                if let (true, Some(mm)) = (is_paint, world.get::<MeshMaterial3d<StandardMaterial>>(e)) {
                    let h = mm.0.clone();
                    if !found.iter().any(|(x, _, _): &(Handle<StandardMaterial>, Color, f32)| *x == h)
                        && let Some(m) = world.resource::<Assets<StandardMaterial>>().get(&h)
                    {
                        found.push((h, m.base_color, m.metallic));
                    }
                }
            }
            world.entity_mut(model).insert(PaintOrig(found));
        }
        let orig = world.get::<PaintOrig>(model).map(|o| o.0.clone()).unwrap_or_default();
        let livery = paint == garage::CARS[0].paint;
        let mut mats = world.resource_mut::<Assets<StandardMaterial>>();
        for (h, base, metallic) in orig {
            let Some(mut m) = mats.get_mut(&h) else { continue };
            if livery {
                m.base_color = base;
                m.metallic = metallic;
            } else {
                let c = PAINTS[paint.colour as usize % PAINTS.len()].1;
                m.base_color = Color::srgb(c[0], c[1], c[2]);
                m.metallic = if paint.metallic { 0.45 } else { 0.0 };
            }
        }
        world.entity_mut(model).remove::<WantsPaint>();
    }
}

/// Camera position and look target for a framing at orbit `angle` (degrees).
fn framing(shot: Shot, angle: f32) -> (Vec3, Vec3, f32) {
    // (distance, height, look height, sideways shift of the look point: + puts the car right).
    let (d, h, look_h, shift, a) = match shot {
        Shot::Orbit => (8.4, 1.45, 0.6, 2.4, angle),
        Shot::Front => (9.4, 1.9, 0.62, 0.0, 145.0),
        Shot::Part(a) => (8.6, 1.2, 0.6, 0.0, a),
    };
    let r = a.to_radians();
    let pos = Vec3::new(r.sin() * d, h, r.cos() * d);
    let target = Vec3::new(0.0, look_h, 0.0);
    let fwd = (target - pos).normalize();
    let right = fwd.cross(Vec3::Y).normalize();
    (pos, target - right * shift, a)
}

fn frame(world: &mut World) {
    let dt = world.resource::<Time<Real>>().delta_secs().min(0.1);
    let shot = *world.resource::<Shot>();
    let still = super::hub::reduce_motion(world);
    let mut q = world.query::<(&mut ShowroomCam, &mut Transform)>();
    for (mut cam, mut tf) in q.iter_mut(world) {
        let shot = if still && shot == Shot::Orbit { Shot::Front } else { shot };
        if shot == Shot::Orbit {
            cam.angle = (cam.angle + 9.0 * dt) % 360.0;
        }
        let (pos, look, a) = framing(shot, cam.angle);
        if shot != Shot::Orbit {
            cam.angle = a;
        }
        if !cam.started || still {
            cam.pos = pos;
            cam.look = look;
            cam.started = true;
        } else {
            // About 0.6 s to settle (exponential ease; the orbit itself is slow).
            let k = 1.0 - (-dt * 7.0).exp();
            cam.pos = cam.pos.lerp(pos, k);
            cam.look = cam.look.lerp(look, k);
        }
        let want = Transform::from_translation(cam.pos).looking_at(cam.look, Vec3::Y);
        if *tf != want {
            *tf = want;
        }
    }
}

/// Fades the veil once the car is in and two frames have drawn (or after 6 s regardless).
fn fade(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let ready = {
        let mut q = world.query_filtered::<(), With<crate::art::car::CarRig>>();
        let rigged = q.iter(world).next().is_some();
        let mut p = world.query::<&Plinth>();
        let traffic_body = p.iter(world).next().and_then(|p| p.shown.as_ref()).is_some_and(|s| garage::def(&s.model).is_some_and(|d| d.body != Body::Rally));
        rigged || traffic_body
    };
    let dur = if super::hub::reduce_motion(world) { 0.15 } else { 0.5 };
    let mut q = world.query::<(Entity, &mut ShowroomVeil, &mut BackgroundColor)>();
    let mut done = None;
    for (e, mut v, mut bg) in q.iter_mut(world) {
        if v.fading.is_none() {
            if ready || now - v.born > 6.0 {
                v.frames += 1;
            }
            if v.frames >= 2 {
                v.fading = Some(now);
            }
            continue;
        }
        let k = ((now - v.fading.unwrap_or(now)) / dur).clamp(0.0, 1.0);
        bg.0.set_alpha(1.0 - k);
        if k >= 1.0 {
            done = Some(e);
        }
    }
    if let Some(e) = done {
        // The contour picture fades with it (children carry their own colour): just remove.
        world.entity_mut(e).despawn();
        return;
    }
    // Fade the veil's picture with it.
    let alpha = {
        let mut q = world.query::<&ShowroomVeil>();
        q.iter(world).next().and_then(|v| v.fading.map(|f| 1.0 - ((now - f) / dur).clamp(0.0, 1.0)))
    };
    if let Some(a) = alpha {
        let mut q = world.query_filtered::<(&mut ImageNode, &ChildOf), Without<ShowroomVeil>>();
        let veils: Vec<Entity> = world.query_filtered::<Entity, With<ShowroomVeil>>().iter(world).collect();
        for (mut img, parent) in q.iter_mut(world) {
            if veils.contains(&parent.parent()) {
                img.color.set_alpha(0.6 * a);
            }
        }
    }
}

/// The name of `paint` on `car` (the Saker's paint 0 is its livery).
pub fn paint_name(car: &str, paint: Paint) -> &'static str {
    let rally = garage::def(car).is_some_and(|d| d.body == Body::Rally);
    if rally && paint.colour == 0 {
        return "Works livery";
    }
    PAINTS[paint.colour as usize % PAINTS.len()].0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framings_look_at_the_car() {
        for shot in [Shot::Orbit, Shot::Front, Shot::Part(90.0)] {
            let (pos, look, _) = framing(shot, 30.0);
            assert!(pos.y > 0.5 && pos.length() > 3.5, "{shot:?} outside the car");
            assert!(look.distance(Vec3::Y * 0.55) < 2.5, "{shot:?} looks near the car");
        }
        // The hub puts the car right of centre: the look point is left of it.
        let (pos, look, _) = framing(Shot::Orbit, 0.0);
        let right = (look - pos).normalize().cross(Vec3::Y);
        assert!((look - Vec3::Y * 0.55).dot(right) < 0.0);
    }
}
